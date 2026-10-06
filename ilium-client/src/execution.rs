//! Process composition root for shared finite CPU and I/O work.
use ilium_execution::{
    Client, ClientLimits, Execution, ExecutionConfig, JobCost, LaneConfig, QuotaGroup, QuotaLimits,
    RejectReason, ShutdownMode, StartError, WorkerPriority,
};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
const MIB: usize = 1024 * 1024;
const CPU_THREADS: usize = 2;
const IO_THREADS: usize = 4;
const SERVICE_THREADS: usize = 1;
// The selected feature owners outside this bank contribute 21 roles:
// clipboard3, media1, presenter1, animation1, icons1, voice4, video10.
// Supervisor/logger/input add3, the opener reaper1, and the existing Tokio
// runtime6. Derive the ceiling from the actual bank configuration; retiring
// owners compete with these same roles and the unchanged4096 MiB allowance.
// This declared scenario does not bound unadmitted libraries or allocator RSS.
const PROCESS_WORKER_THREADS: usize = CPU_THREADS + IO_THREADS + SERVICE_THREADS + 21 + 3 + 1 + 6;
// The terminal parser reserves twice `terminal.engine_memory_budget_mib` (engines
// plus published snapshots) on top of the fixed allowance; the setting's maximum
// bounds that extra declaration.
const PROCESS_WORKER_BYTES: usize =
    (4096 + 2 * crate::config::TerminalSettings::MAX_ENGINE_MEMORY_BUDGET_MIB as usize) * MIB;

pub(crate) fn admission_notification() -> Arc<tokio::sync::Notify> {
    static WAKE: OnceLock<Arc<tokio::sync::Notify>> = OnceLock::new();
    Arc::clone(WAKE.get_or_init(|| Arc::new(tokio::sync::Notify::new())))
}

/// All explicit execution owners in this client process share this primitive
/// budget. It owns no threads or engines and never substitutes for lifecycle.
pub(crate) fn process_quota() -> QuotaGroup {
    static QUOTA: OnceLock<QuotaGroup> = OnceLock::new();
    QUOTA
        .get_or_init(|| {
            let wake = admission_notification();
            QuotaGroup::new_with_admission_wake(
                QuotaLimits {
                    clients: 32,
                    jobs: 64,
                    service_jobs: 1,
                    // General children collectively512 MiB; independent
                    // decoder128 + encoder128 MiB retain dedicated headroom.
                    input_bytes: 768 * MIB,
                    result_bytes: 768 * MIB,
                    worker_threads: PROCESS_WORKER_THREADS,
                    // Shared ceiling for selected charged owners; retiring native
                    // helpers must retain and compete for their original debit.
                    // This is a declaration ceiling, not measured process RSS.
                    worker_bytes: PROCESS_WORKER_BYTES,
                },
                move || wake.notify_waiters(),
            )
        })
        .clone()
}

/// One cadence ledger for every native source consumer in this client process,
/// including preview. It owns no bank, thread or independent quota.
pub(crate) fn native_source_cadence(
) -> Result<Arc<ilium_animation_js::native_source_host::SourceCadence>, String> {
    static CADENCE: OnceLock<
        Mutex<Option<Arc<ilium_animation_js::native_source_host::SourceCadence>>>,
    > = OnceLock::new();
    let mut slot = CADENCE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "Native source cadence lock poisoned".to_owned())?;
    if let Some(cadence) = slot.as_ref() {
        return Ok(Arc::clone(cadence));
    }
    let cadence = Arc::new(
        ilium_animation_js::native_source_host::SourceCadence::new(process_quota())
            .map_err(|error| error.to_string())?,
    );
    *slot = Some(Arc::clone(&cadence));
    Ok(cadence)
}

/// Real process bootstrap, before logging or other platform-owned workers.
/// Repeated CLI/client calls preserve the existing quota identity and one
/// permanent supervisor charge. Ordinary fixture-bank startup does not call it.
pub fn bootstrap_process_quota() -> std::io::Result<QuotaGroup> {
    let quota = process_quota();
    ilium_execution::initialize_process_supervisor(&quota).map_err(std::io::Error::other)?;
    Ok(quota)
}

/// CLI bootstrap only, before building its existing runtime. Ordinary client
/// fixture banks must not install a permanent runtime declaration.
pub fn bootstrap_runtime_admission(
    thread_capacity: usize,
    stack_bytes: usize,
) -> std::io::Result<()> {
    ilium_execution::initialize_process_runtime_admission(
        &process_quota(),
        thread_capacity,
        stack_bytes,
    )
    .map(|_| ())
    .map_err(std::io::Error::other)
}

fn shared_admission_group(
    identity: &OnceLock<ilium_execution::AdmissionGroup>,
    limits: ClientLimits,
) -> Result<ilium_execution::AdmissionGroup, RejectReason> {
    if let Some(group) = identity.get() {
        return Ok(group.clone());
    }
    let group = process_quota().admission_group(limits)?;
    // Losing concurrent registrations never escape as independent identities.
    let _ = identity.set(group);
    identity.get().cloned().ok_or(RejectReason::InvalidCost)
}

pub(crate) fn general_admission_group() -> Result<ilium_execution::AdmissionGroup, RejectReason> {
    static GENERAL: OnceLock<ilium_execution::AdmissionGroup> = OnceLock::new();
    shared_admission_group(
        &GENERAL,
        ClientLimits {
            jobs: 60,
            service_jobs: 1,
            input_bytes: 512 * MIB,
            result_bytes: 512 * MIB,
        },
    )
}

pub(crate) enum CodecDirection {
    Decoder,
    Encoder,
}
pub(crate) fn codec_admission_group(
    direction: CodecDirection,
) -> Result<ilium_execution::AdmissionGroup, RejectReason> {
    static DECODER: OnceLock<ilium_execution::AdmissionGroup> = OnceLock::new();
    static ENCODER: OnceLock<ilium_execution::AdmissionGroup> = OnceLock::new();
    let identity = match direction {
        CodecDirection::Decoder => &DECODER,
        CodecDirection::Encoder => &ENCODER,
    };
    shared_admission_group(
        identity,
        ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 128 * MIB,
            result_bytes: 128 * MIB,
        },
    )
}

pub struct ClientExecution {
    execution: Execution,
    general: Client,
    location_search: Client,
}

/// Initialized identities and the actual partial bank survive every refusal.
#[derive(Default)]
struct ClientStartupState {
    execution: Option<Execution>,
    general: Option<Client>,
    location_search: Option<Client>,
    primary: Option<StartError>,
    observation: Option<Result<ilium_execution::JoinReport, ilium_execution::JoinUseError>>,
}
impl ClientStartupState {
    fn bootstrap(&mut self) {
        tokenizers::utils::parallelism::set_parallelism(false);
        // The unchanged aggregate identity requires no physical bank yet.
        let aggregate = match general_admission_group() {
            Ok(aggregate) => aggregate,
            Err(reason) => {
                self.primary = Some(StartError::Admission(reason));
                return;
            }
        };
        if let Err(error) = self.populate(process_quota(), bank_config(), &aggregate) {
            self.primary = Some(error);
            self.observe_cleanup_background(Instant::now() + Duration::from_secs(5));
        }
    }
    fn populate(
        &mut self,
        quota: QuotaGroup,
        config: ExecutionConfig,
        aggregate: &ilium_execution::AdmissionGroup,
    ) -> Result<(), StartError> {
        match Execution::start_with_custody(quota, config) {
            Ok(execution) => self.execution = Some(execution),
            Err(failure) => {
                let (error, execution) = failure.into_parts();
                self.execution = execution;
                return Err(error);
            }
        }
        self.general = Some(
            self.execution
                .as_ref()
                .expect("started bank")
                .client_in_group(
                    aggregate,
                    ClientLimits {
                        jobs: 60,
                        service_jobs: 1,
                        input_bytes: 512 * MIB,
                        result_bytes: 512 * MIB,
                    },
                )
                .map_err(StartError::Admission)?,
        );
        self.location_search = Some(
            self.general
                .as_ref()
                .expect("general original admitted")
                .child(ClientLimits {
                    jobs: 1,
                    service_jobs: 0,
                    input_bytes: 16 * MIB,
                    result_bytes: 4 * MIB,
                })
                .map_err(StartError::Admission)?,
        );
        Ok(())
    }
    fn observe_cleanup_background(&mut self, deadline: Instant) {
        let Some(execution) = self.execution.as_mut() else {
            return;
        };
        execution.request_shutdown(ShutdownMode::Cancel);
        let observation = execution.join_until_background(deadline);
        let complete = matches!(&observation, Ok(report) if report.shutdown_complete);
        self.observation = Some(observation);
        if complete {
            // No semantic jobs are accepted during populate. Release every
            // initialized identity only after the original bank physically joined.
            self.location_search = None;
            self.general = None;
            self.execution = None;
        }
    }
    fn finish_cancelled_bootstrap(&mut self) {
        // Startup accepted no jobs. Keep the actual bank on this existing
        // runtime blocking owner until its native workers physically exit.
        while self.execution.is_some() {
            self.observe_cleanup_background(Instant::now() + Duration::from_secs(5));
        }
    }
}

struct StartupTransfer(std::sync::mpsc::SyncSender<()>);
impl StartupTransfer {
    fn accept(self) {
        let _ = self.0.send(());
    }
}
// Dropping the sender means cancellation. The bootstrap owner, rather than a
// detached result or a second observer task, owns and joins the actual bank.
struct StartupBackground {
    state: Arc<Mutex<ClientStartupState>>,
    transferred: bool,
}
impl Drop for StartupBackground {
    fn drop(&mut self) {
        if !self.transferred {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .finish_cancelled_bootstrap();
        }
    }
}

/// A lost observer or failed join retains the actual bank, not just its text.
pub struct ClientExecutionStartError {
    state: Arc<Mutex<ClientStartupState>>,
    observer: Option<tokio::task::JoinError>,
}
impl ClientExecutionStartError {
    pub fn with_primary_error<R>(&self, inspect: impl FnOnce(Option<&StartError>) -> R) -> R {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        inspect(state.primary.as_ref())
    }
    pub fn retains_execution(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .execution
            .is_some()
    }
    pub fn is_retryable_busy(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        self.observer.is_none()
            && state.execution.is_none()
            && state.general.is_none()
            && state.location_search.is_none()
            && matches!(
                state.primary,
                Some(StartError::Admission(RejectReason::Busy))
            )
    }
    /// Explicit exceptional observation on a background owner, never the UI.
    pub fn observe_cleanup_background(&self, deadline: Instant) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.observe_cleanup_background(deadline);
        state.execution.is_none()
    }
}
impl std::fmt::Debug for ClientExecutionStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        f.debug_struct("ClientExecutionStartError")
            .field("primary", &state.primary)
            .field("partial_bank_retained", &state.execution.is_some())
            .field("cleanup_observation", &state.observation)
            .field("observer", &self.observer)
            .finish()
    }
}
impl std::fmt::Display for ClientExecutionStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(primary) = &state.primary {
            write!(f, "client bank startup: {primary}")?;
        }
        if let Some(observer) = &self.observer {
            write!(f, "; startup observer failed: {observer}")?;
        }
        if state.execution.is_some() {
            f.write_str("; actual partial bank retained")?;
        }
        Ok(())
    }
}
impl std::error::Error for ClientExecutionStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.observer.as_ref().map(|error| error as _)
    }
}

// One source of truth for constructor and selected-composition admission.
fn bank_config() -> ExecutionConfig {
    let bank = |threads, resident_bytes_per_thread| LaneConfig {
        threads,
        queue_slots: if threads == 0 { 0 } else { 16 },
        priority: Some(WorkerPriority::BelowNormal),
        resident_bytes_per_thread,
    };
    ExecutionConfig {
        cpu: bank(CPU_THREADS, 64 * MIB),
        io: bank(IO_THREADS, MIB),
        service: LaneConfig {
            threads: SERVICE_THREADS,
            queue_slots: 1,
            priority: Some(WorkerPriority::BelowNormal),
            resident_bytes_per_thread: 128 * MIB,
        },
    }
}

impl ClientExecution {
    /// Bootstrap only: creates seven bank threads before interaction.
    /// Production calls bootstrap_process_quota before logging; independent
    /// fixture banks do not designate the process supervisor. Interactive
    /// codecs use this bank with the original directional admission groups.
    pub fn start() -> Result<Self, ClientExecutionStartError> {
        let mut state = ClientStartupState::default();
        state.bootstrap();
        Self::finish_startup(Arc::new(Mutex::new(state)), None)
    }
    /// Native bootstrap runs off the interactive loop on its existing runtime.
    /// Only completion transfers through the observer; the shared slot owns
    /// all initialized originals even if the observer returns an error.
    pub async fn start_async() -> Result<Self, ClientExecutionStartError> {
        Self::start_async_with(ClientStartupState::bootstrap, || {}).await
    }
    async fn start_async_with(
        bootstrap: impl FnOnce(&mut ClientStartupState) + Send + 'static,
        finished: impl FnOnce() + Send + 'static,
    ) -> Result<Self, ClientExecutionStartError> {
        let state = Arc::new(Mutex::new(ClientStartupState::default()));
        let background = Arc::clone(&state);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (transfer_tx, transfer_rx) = std::sync::mpsc::sync_channel(1);
        let transfer = StartupTransfer(transfer_tx);
        let observer = tokio::task::spawn_blocking(move || {
            let mut owner = StartupBackground {
                state: background,
                transferred: false,
            };
            bootstrap(
                &mut owner
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            );
            let announced = ready_tx.send(()).is_ok();
            owner.transferred = announced && transfer_rx.recv().is_ok();
            drop(owner);
            finished();
        });
        if ready_rx.await.is_err() {
            // The native bootstrap panicked: the exact shared slots survive
            // through the typed observer error, including poisoned state.
            return Self::finish_startup(state, observer.await.err());
        }
        let result = Self::finish_startup(state, None);
        transfer.accept();
        result
    }
    fn finish_startup(
        state: Arc<Mutex<ClientStartupState>>,
        observer: Option<tokio::task::JoinError>,
    ) -> Result<Self, ClientExecutionStartError> {
        if observer.is_some() {
            return Err(ClientExecutionStartError { state, observer });
        }
        let mut original = state.lock().unwrap_or_else(|error| error.into_inner());
        if original.primary.is_some() {
            drop(original);
            return Err(ClientExecutionStartError {
                state,
                observer: None,
            });
        }
        // Every fallible admission stored its owner before the next stage.
        let execution = original.execution.take().expect("completed bank bootstrap");
        let general = original
            .general
            .take()
            .expect("completed general admission");
        let location_search = original
            .location_search
            .take()
            .expect("completed location admission");
        Ok(Self {
            execution,
            general,
            location_search,
        })
    }
    pub fn client(&self, limits: ClientLimits) -> Result<Client, RejectReason> {
        self.general.child(limits)
    }
    /// Codec jobs share the existing CPU queue but retain their independent
    /// process-wide decoder/encoder envelopes. Do not nest either under the
    /// general client's 512 MiB aggregate or create another physical bank.
    pub(crate) fn ipc_preparation(
        &self,
        outbound: Client,
    ) -> Result<crate::ipc_preparation::IpcPreparation, RejectReason> {
        if !outbound
            .quota_group()
            .shares_root(&self.general.quota_group())
        {
            return Err(RejectReason::InvalidCost);
        }
        if !outbound.is_open() {
            return Err(RejectReason::Closed);
        }
        let decoder_group = codec_admission_group(CodecDirection::Decoder)?;
        let encoder_group = codec_admission_group(CodecDirection::Encoder)?;
        let decoder = self
            .execution
            .client_in_group(&decoder_group, crate::ipc_preparation::codec_limits())?;
        let encoder = self
            .execution
            .client_in_group(&encoder_group, crate::ipc_preparation::codec_limits())?;
        Ok(crate::ipc_preparation::IpcPreparation::new(
            decoder, encoder, outbound,
        ))
    }
    /// One child of the existing aggregate general client; no ambient bank or
    /// quota is created. Adapter peaks must fit explicit job/storage admission.
    pub fn ambient_resources(
        &self,
    ) -> Result<ilium_ambient::resources::AmbientResources, RejectReason> {
        let finite = self.client(ClientLimits {
            jobs: 4,
            service_jobs: 0,
            input_bytes: 128 * MIB,
            result_bytes: 128 * MIB,
        })?;
        Ok(ilium_ambient::resources::AmbientResources::new(finite))
    }
    /// Clones the one geocoding tenant on this execution's existing I/O bank.
    /// A blocked lookup keeps its job charged across picker replacement.
    pub fn location_search(&self) -> Client {
        self.location_search.clone()
    }
    /// Clone this one client identity for Markdown, source windows and syntax.
    /// All adapters share its existing aggregate credits and physical bank.
    pub fn documents(&self) -> Result<Client, RejectReason> {
        self.client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 384 * MIB,
            result_bytes: 256 * MIB,
        })
    }
    pub fn terminal_parser(&self) -> Result<Client, RejectReason> {
        self.client(ClientLimits {
            jobs: 1,
            service_jobs: 1,
            input_bytes: 128 * MIB,
            result_bytes: 2 * MIB,
        })
    }
    /// The fixed banks do computation; this single tracked blocking task only
    /// observes their actual joins. A deadline never claims a hung worker exited.
    pub async fn shutdown(mut self) -> Result<(), std::io::Error> {
        self.execution.request_shutdown(ShutdownMode::Cancel);
        let observation = tokio::task::spawn_blocking(move || {
            self.execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
        });
        let report = observation
            .await
            .map_err(std::io::Error::other)?
            .map_err(|e| std::io::Error::other(format!("execution shutdown: {e:?}")))?;
        if report.remaining_workers != 0 {
            return Err(std::io::Error::other(format!(
                "execution shutdown deadline: {} workers still owned",
                report.remaining_workers
            )));
        }
        if !report.shutdown_complete {
            let cpu = &report.health.lanes[0];
            return Err(std::io::Error::other(format!(
                "execution shutdown incomplete: {} retirement originals live, {} in recovery custody; admitted work remains",
                cpu.retirement_live, cpu.retirement_recovery_pending
            )));
        }
        Ok(())
    }
}
/// Conservative peak declaration includes library temporary allocations;
/// allocator/RSS limits remain cooperative rather than enforced by an arena.
pub(crate) const DOCUMENT_COST: JobCost = JobCost {
    input_bytes: 64 * MIB,
    result_bytes: 32 * MIB,
};

/// Shared real bank for ordinary fixtures. The process retains its owner;
/// callers clone one identity and never hold this mutex while driving receipts.
#[cfg(test)]
fn test_bank() -> &'static std::sync::Mutex<(ClientExecution, Client)> {
    static BANK: std::sync::OnceLock<std::sync::Mutex<(ClientExecution, Client)>> =
        std::sync::OnceLock::new();
    BANK.get_or_init(|| {
        // Nonblocking admission may race another fixture's quota update. Retry
        // only Busy during bootstrap; limits and structural failures stay fatal.
        let deadline = Instant::now() + Duration::from_secs(5);
        let execution = loop {
            match ClientExecution::start() {
                Ok(execution) => break execution,
                Err(error) if error.is_retryable_busy() && Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                Err(error) => panic!("shared test bank startup: {error}"),
            }
        };
        let client = loop {
            match execution.client(ClientLimits {
                jobs: 64,
                service_jobs: 0,
                input_bytes: 512 * MIB,
                result_bytes: 512 * MIB,
            }) {
                Ok(client) => break client,
                Err(RejectReason::Busy) if Instant::now() < deadline => std::thread::yield_now(),
                Err(error) => panic!("shared test client admission: {error:?}"),
            }
        };
        std::sync::Mutex::new((execution, client))
    })
}

#[cfg(test)]
pub(crate) fn test_client() -> Client {
    test_bank()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .1
        .clone()
}

/// Retention assertions require a distinct tenant, while sharing the same OS
/// bank. Ordinary fixtures continue to clone the single general identity.
#[cfg(test)]
pub(crate) fn test_document_client() -> Client {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = test_bank()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .0
            .documents();
        match result {
            Ok(client) => return client,
            Err(RejectReason::Busy) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => panic!("document test tenant admission: {error:?}"),
        }
    }
}

#[cfg(test)]
mod composition_tests {
    use super::*;

    #[test]
    fn historical_selected_subset_preserves_storage_boundary() {
        let quota = QuotaGroup::new(QuotaLimits {
            worker_threads: 16,
            worker_bytes: 2304 * MIB,
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
        });
        let bank = quota.reserve_external_worker(5, 258 * MIB).unwrap();
        let parser = quota.reserve_external_storage(512 * MIB).unwrap();
        let clipboard = quota.reserve_external_worker(3, 264 * MIB).unwrap();
        let media = quota.reserve_external_worker(1, 400 * MIB).unwrap();
        let icons = quota.reserve_external_worker(1, 512 * MIB).unwrap();
        let presenter = quota.reserve_external_worker(1, 96 * MIB).unwrap();
        let frames = quota.reserve_external_storage(97 * MIB).unwrap();
        let retiring_animation = quota.reserve_external_worker(4, 96 * MIB).unwrap();
        assert_eq!(quota.snapshot().worker_threads, 15);
        assert!(matches!(
            quota.reserve_external_storage(128 * MIB),
            Err(RejectReason::WorkerBytes)
        ));
        drop((
            bank,
            parser,
            clipboard,
            media,
            icons,
            presenter,
            frames,
            retiring_animation,
        ));
        assert_eq!(quota.snapshot().worker_bytes, 0);
        assert_eq!(quota.snapshot().worker_threads, 0);
    }

    #[test]
    fn actual_bank_admission_fits_selected_roles_and_releases_after_native_join() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 32,
            jobs: 64,
            service_jobs: 1,
            input_bytes: 768 * MIB,
            result_bytes: 768 * MIB,
            worker_threads: PROCESS_WORKER_THREADS,
            worker_bytes: PROCESS_WORKER_BYTES,
        });
        let config = bank_config();
        let bank_threads = config.cpu.threads + config.io.threads + config.service.threads;
        let mut execution = Execution::start(quota.clone(), config).unwrap();
        assert_eq!(quota.snapshot().worker_threads, bank_threads);
        // Other roles are declared here; their actual engines/children are
        // covered by separate lifecycle qualification. This bank is native.
        let others = quota
            .reserve_external_worker(PROCESS_WORKER_THREADS - bank_threads, 3191 * MIB)
            .unwrap();
        assert_eq!(quota.snapshot().worker_threads, PROCESS_WORKER_THREADS);
        assert!(matches!(
            quota.reserve_external_worker(1, 1),
            Err(RejectReason::WorkerLimit)
        ));
        drop(others);
        execution.request_shutdown(ShutdownMode::Drain);
        let joined = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(joined.remaining_workers, 0);
        drop(execution);
        assert_eq!(quota.snapshot().worker_threads, 0);
    }

    #[test]
    fn selected_single_video_owner_declarations_fit_and_release() {
        let quota = QuotaGroup::new(QuotaLimits {
            worker_threads: PROCESS_WORKER_THREADS,
            worker_bytes: PROCESS_WORKER_BYTES,
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
        });
        // Source-grounded declarations, not a claim that every owner is active
        // simultaneously or that unadmitted library threads are accounted here.
        let config = bank_config();
        let bank_threads = config.cpu.threads + config.io.threads + config.service.threads;
        let bank_bytes = config.cpu.threads * config.cpu.resident_bytes_per_thread
            + config.io.threads * config.io.resident_bytes_per_thread
            + config.service.threads * config.service.resident_bytes_per_thread;
        let bank = quota
            .reserve_external_worker(bank_threads, bank_bytes)
            .unwrap();
        let parser = quota.reserve_external_storage(512 * MIB).unwrap();
        let clipboard = quota.reserve_external_worker(3, 264 * MIB).unwrap();
        let media = quota.reserve_external_worker(1, 400 * MIB).unwrap();
        let presenter = quota.reserve_external_worker(1, 96 * MIB).unwrap();
        let presentation = quota.reserve_external_storage(97 * MIB).unwrap();
        let animation = quota.reserve_external_worker(1, 0).unwrap();
        let animation_frames = quota.reserve_external_storage(3 * 24 * MIB).unwrap();
        let icons = quota.reserve_external_worker(1, 512 * MIB).unwrap();
        let voice = quota.reserve_external_worker(4, 70 * MIB).unwrap();
        let voice_custody = quota.reserve_external_storage(128 * 1024).unwrap();
        let video = quota.reserve_external_worker(10, 1064 * MIB).unwrap();
        let video_storage = quota.reserve_external_storage(104 * MIB).unwrap();
        assert_eq!(quota.snapshot().worker_threads, bank_threads + 21);
        assert_eq!(
            quota.snapshot().worker_bytes,
            3191 * MIB + bank_bytes + 128 * 1024
        );
        // These real bootstrap declarations are additive to the
        // selected census from the constructor configuration. The4096 MiB
        // storage ceiling stays fixed; codecs add no separate bank thread.
        let supervisor_bytes = ilium_platform::owned_worker::supervisor_declared_bytes();
        let log_path = std::path::Path::new("selected-session.log");
        let supervisor = quota.reserve_external_worker(1, supervisor_bytes).unwrap();
        let logger = quota
            .reserve_external_worker(1, ilium_logging::LOGGER_STACK_BYTES)
            .unwrap();
        let logger_storage = quota
            .reserve_external_storage(ilium_logging::logger_storage_bytes(log_path).unwrap())
            .unwrap();
        let input = quota
            .reserve_external_worker(1, crate::terminal_input_owner::INPUT_STACK_BYTES)
            .unwrap();
        let input_storage = quota
            .reserve_external_storage(crate::terminal_input_owner::input_storage_bytes())
            .unwrap();
        // Match the external owner's full resident declaration, including
        // its requested stack and bounded child registry. The existing Tokio
        // runtime admits its six async/blocking roles before construction.
        let opener_bytes = crate::external_open::REAPER_BYTES;
        let opener = quota.reserve_external_worker(1, opener_bytes).unwrap();
        let runtime_bytes = 6 * 2 * MIB;
        let runtime = quota.reserve_external_worker(6, runtime_bytes).unwrap();
        assert_eq!(quota.snapshot().worker_threads, PROCESS_WORKER_THREADS);
        assert_eq!(
            quota.snapshot().worker_bytes,
            3191 * MIB
                + bank_bytes
                + 128 * 1024
                + supervisor_bytes
                + ilium_logging::LOGGER_STACK_BYTES
                + ilium_logging::logger_storage_bytes(log_path).unwrap()
                + crate::terminal_input_owner::INPUT_STACK_BYTES
                + crate::terminal_input_owner::input_storage_bytes()
                + opener_bytes
                + runtime_bytes
        );
        // A free native domain slot cannot bypass shared physical admission.
        assert!(matches!(
            quota.reserve_external_worker(1, 1),
            Err(RejectReason::WorkerLimit)
        ));
        let filler = quota
            .reserve_external_storage(
                PROCESS_WORKER_BYTES - quota.snapshot().worker_bytes - 127 * MIB,
            )
            .unwrap();
        assert!(matches!(
            quota.reserve_external_storage(128 * MIB),
            Err(RejectReason::WorkerBytes)
        ));
        drop((
            bank,
            parser,
            clipboard,
            media,
            presenter,
            presentation,
            animation,
            animation_frames,
            icons,
            voice,
            voice_custody,
            video,
            video_storage,
            supervisor,
            logger,
            logger_storage,
            input,
            input_storage,
            opener,
            runtime,
            filler,
        ));
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    #[ignore = "run this process-global identity test alone with --ignored --exact --test-threads=1"]
    fn two_interactive_clients_share_one_bank_and_distinct_codec_groups() {
        let bank = test_bank()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let execution = &bank.0;
        let root = process_quota();
        let general = general_admission_group().expect("general identity");
        let decoder = codec_admission_group(CodecDirection::Decoder).expect("decoder identity");
        let encoder = codec_admission_group(CodecDirection::Encoder).expect("encoder identity");
        let before_threads = root.snapshot().worker_threads;
        let before_general = general.usage().clients;
        let before_decoder = decoder.usage().clients;
        let before_encoder = encoder.usage().clients;
        let outbound_limits = ClientLimits {
            jobs: 1,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
        };
        let first = execution
            .ipc_preparation(execution.client(outbound_limits).expect("first outbound"))
            .expect("first interactive connection");
        let second = execution
            .ipc_preparation(execution.client(outbound_limits).expect("second outbound"))
            .expect("second interactive connection");
        assert_eq!(
            root.snapshot().worker_threads,
            before_threads,
            "interactive codecs reuse the existing CPU bank"
        );
        assert_eq!(general.usage().clients, before_general + 2);
        assert_eq!(decoder.usage().clients, before_decoder + 2);
        assert_eq!(encoder.usage().clients, before_encoder + 2);
        drop((first, second));
        assert_eq!(general.usage().clients, before_general);
        assert_eq!(decoder.usage().clients, before_decoder);
        assert_eq!(encoder.usage().clients, before_encoder);
    }
}

#[cfg(test)]
#[path = "execution_startup_tests.rs"]
mod execution_startup_tests;
