//! Process composition root for shared finite CPU and I/O work.
use ilium_execution::{
    Client, ClientLimits, Execution, ExecutionConfig, JobCost, LaneConfig, QuotaGroup, QuotaLimits,
    RejectReason, ShutdownMode, StartError, WorkerPriority,
};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
const MIB: usize = 1024 * 1024;
// Preserve the selected 26-role / 3449 MiB + 128 KiB feature composition,
// then count its three previously omitted startup owners: supervisor, logger,
// and terminal input. Interactive codecs reuse this five-thread bank rather
// than adding the standalone codec's sixth thread. Additional/retiring owners
// still compete within 29 roles and the unchanged 4096 MiB storage declaration.
// These declarations do not bound native stacks, allocator RSS, or all OS threads.
const PROCESS_WORKER_THREADS: usize = 29;
const PROCESS_WORKER_BYTES: usize = 4096 * MIB;

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

/// Real process bootstrap, before logging or other platform-owned workers.
/// Repeated CLI/client calls preserve the existing quota identity and one
/// permanent supervisor charge. Ordinary fixture-bank startup does not call it.
pub fn bootstrap_process_quota() -> std::io::Result<QuotaGroup> {
    let quota = process_quota();
    ilium_execution::initialize_process_supervisor(&quota).map_err(std::io::Error::other)?;
    Ok(quota)
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
}
impl ClientExecution {
    /// Bootstrap only: creates five bank threads before interaction.
    /// Production calls bootstrap_process_quota before logging; independent
    /// fixture banks do not designate the process supervisor. Interactive
    /// codecs use this bank with the original directional admission groups.
    pub fn start() -> Result<Self, StartError> {
        // Tokenization executes on its admitted engine owner. Do not create
        // an implicit process-global Rayon pool outside physical admission.
        tokenizers::utils::parallelism::set_parallelism(false);
        let quota = process_quota();
        let bank = |threads, resident_bytes_per_thread| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 16 },
            priority: Some(WorkerPriority::BelowNormal),
            resident_bytes_per_thread,
        };
        let execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: bank(2, 64 * MIB),
                io: bank(2, MIB),
                service: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: Some(WorkerPriority::BelowNormal),
                    resident_bytes_per_thread: 128 * MIB,
                },
            },
        )?;
        let aggregate = general_admission_group().map_err(StartError::Admission)?;
        let general = execution
            .client_in_group(
                &aggregate,
                ClientLimits {
                    jobs: 60,
                    service_jobs: 1,
                    input_bytes: 512 * MIB,
                    result_bytes: 512 * MIB,
                },
            )
            .map_err(StartError::Admission)?;
        Ok(Self { execution, general })
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
                Err(StartError::Admission(RejectReason::Busy)) if Instant::now() < deadline => {
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
        let bank = quota.reserve_external_worker(5, 258 * MIB).unwrap();
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
        assert_eq!(quota.snapshot().worker_threads, 26);
        assert_eq!(quota.snapshot().worker_bytes, 3449 * MIB + 128 * 1024);
        // These three real bootstrap declarations are additive to the
        // previously selected 26-role feature census, without widening the
        // unchanged 4096 MiB storage ceiling or adding a codec bank thread.
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
        assert_eq!(quota.snapshot().worker_threads, PROCESS_WORKER_THREADS);
        assert_eq!(
            quota.snapshot().worker_bytes,
            3449 * MIB
                + 128 * 1024
                + supervisor_bytes
                + ilium_logging::LOGGER_STACK_BYTES
                + ilium_logging::logger_storage_bytes(log_path).unwrap()
                + crate::terminal_input_owner::INPUT_STACK_BYTES
                + crate::terminal_input_owner::input_storage_bytes()
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
