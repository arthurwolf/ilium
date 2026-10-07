//! One finite startup discovery job on the existing I/O bank.
//! The returned original remains charged after installation into the UI.
use ilium_execution::{
    Client, ClientLimits, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, Retained,
};
use std::{io, mem::size_of, sync::Arc};
use tokio::sync::Notify;

const CATALOGUE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct AudioCatalogue {
    pub sounds: ilium_sound::SoundDiscovery,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

pub(crate) fn limits() -> ClientLimits {
    ClientLimits {
        jobs: 1,
        service_jobs: 0,
        input_bytes: CATALOGUE_BYTES,
        result_bytes: CATALOGUE_BYTES,
    }
}

impl AudioCatalogue {
    fn retained_bytes(&self) -> Option<usize> {
        let mut bytes = size_of::<Self>()
            .checked_add(
                self.sounds
                    .directories
                    .capacity()
                    .checked_mul(size_of::<ilium_sound::SoundDirectory>())?,
            )?
            .checked_add(
                self.sounds
                    .sounds
                    .capacity()
                    .checked_mul(size_of::<ilium_sound::SystemSound>())?,
            )?;
        for directory in &self.sounds.directories {
            bytes = bytes
                .checked_add(directory.path.capacity())?
                .checked_add(directory.origin.capacity())?;
        }
        for sound in &self.sounds.sounds {
            bytes = bytes
                .checked_add(sound.path.capacity())?
                .checked_add(sound.display_name.capacity())?
                .checked_add(sound.collection.capacity())?;
        }
        for names in [&self.inputs, &self.outputs] {
            bytes = bytes.checked_add(names.capacity().checked_mul(size_of::<String>())?)?;
            for name in names {
                bytes = bytes.checked_add(name.capacity())?;
            }
        }
        Some(bytes)
    }
}

fn interrupted() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "startup audio discovery cancelled",
    )
}

pub(crate) async fn discover(client: Client) -> io::Result<Retained<AudioCatalogue>> {
    discover_with(client, |context| {
        if context.stop_requested() {
            return Err(interrupted());
        }
        let sounds = ilium_sound::discover_system_sounds();
        if context.stop_requested() {
            return Err(interrupted());
        }
        let inputs = ilium_voice::available_input_devices().unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to enumerate voice input devices");
            Vec::new()
        });
        if context.stop_requested() {
            return Err(interrupted());
        }
        let outputs = ilium_voice::available_output_devices().unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to enumerate voice output devices");
            Vec::new()
        });
        Ok(AudioCatalogue {
            sounds,
            inputs,
            outputs,
        })
    })
    .await
}

async fn discover_with(
    client: Client,
    collect: impl FnOnce(JobContext) -> io::Result<AudioCatalogue> + Send + 'static,
) -> io::Result<Retained<AudioCatalogue>> {
    let completed = Arc::new(Notify::new());
    let wake = Arc::clone(&completed);
    let client = client.with_completion_wake(move || wake.notify_one());
    // Admission precedes every filesystem/native-library call. The fixed
    // declaration and output audit do not bound opaque library scratch or the
    // environment-root collector; those producers still need capped APIs.
    let receipt = client
        .try_submit(
            Lane::Io,
            JobCost {
                input_bytes: CATALOGUE_BYTES,
                result_bytes: CATALOGUE_BYTES,
            },
            move |context: JobContext| {
                let catalogue = collect(context)?;
                if catalogue
                    .retained_bytes()
                    .is_none_or(|bytes| bytes > CATALOGUE_BYTES)
                {
                    return Err(io::Error::other(
                        "startup audio catalogue exceeds its storage declaration",
                    ));
                }
                Ok(catalogue)
            },
        )
        .map_err(|rejected| {
            io::Error::other(format!(
                "startup audio admission refused: {:?}",
                rejected.reason
            ))
        })?;
    let mut receipt = CancelOnDrop(receipt);
    loop {
        let notified = completed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        match receipt.0.try_take() {
            JobPoll::Pending => notified.await,
            JobPoll::Ready(outcome) => {
                return match outcome.view() {
                    JobOutcome::Finished(Ok(_)) => Ok(outcome.map(|outcome| match outcome {
                        JobOutcome::Finished(Ok(catalogue)) => catalogue,
                        _ => unreachable!("validated exclusive startup outcome"),
                    })),
                    JobOutcome::Finished(Err(_)) => Err(io::Error::other(DiscoveryFailure(
                        outcome.map(|outcome| match outcome {
                            JobOutcome::Finished(Err(error)) => error,
                            _ => unreachable!("validated exclusive discovery error"),
                        }),
                    ))),
                    JobOutcome::NotStarted { .. } => Err(interrupted()),
                    JobOutcome::Panicked => {
                        Err(io::Error::other("startup audio discovery worker panicked"))
                    }
                }
            }
            JobPoll::Lost | JobPoll::Taken => {
                return Err(io::Error::other("startup audio discovery completion lost"))
            }
        }
    }
}

struct DiscoveryFailure(Retained<io::Error>);
impl std::fmt::Debug for DiscoveryFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.0.view(), formatter)
    }
}
impl std::fmt::Display for DiscoveryFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.0.view(), formatter)
    }
}
impl std::error::Error for DiscoveryFailure {}
struct CancelOnDrop<J: ilium_execution::Job>(Receipt<J>);
impl<J: ilium_execution::Job> Drop for CancelOnDrop<J> {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };

    fn bank() -> (Execution, Client, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: CATALOGUE_BYTES * 2,
            result_bytes: CATALOGUE_BYTES * 2,
            worker_threads: 1,
            worker_bytes: 1024 * 1024,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: disabled,
                service: disabled,
                io: LaneConfig {
                    threads: 1,
                    queue_slots: 2,
                    priority: None,
                    resident_bytes_per_thread: 4096,
                },
            },
        )
        .unwrap();
        let client = execution.client(limits()).unwrap();
        (execution, client, quota)
    }
    fn empty() -> AudioCatalogue {
        AudioCatalogue {
            sounds: Default::default(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }
    fn join(mut execution: Execution) {
        execution.request_shutdown(ShutdownMode::Cancel);
        let report = std::thread::spawn(move || {
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
        })
        .join()
        .unwrap();
        assert!(report.shutdown_complete);
    }

    #[tokio::test]
    async fn native_io_catalogue_moves_originals_with_their_storage_charge() {
        let (execution, client, quota) = bank();
        let caller = std::thread::current().id();
        let result = discover_with(client, move |_| {
            assert_ne!(std::thread::current().id(), caller);
            let mut catalogue = empty();
            catalogue.inputs = vec!["original microphone".into()];
            Ok(catalogue)
        })
        .await
        .unwrap();
        let (catalogue, retention) = result.into_parts();
        assert_eq!(catalogue.inputs, ["original microphone"]);
        assert_eq!(quota.snapshot().jobs, 1);
        drop(catalogue);
        assert_eq!(quota.snapshot().jobs, 1);
        drop(retention);
        assert_eq!(quota.snapshot().jobs, 0);
        join(execution);
    }

    #[tokio::test]
    async fn admission_refusal_does_not_enter_native_discovery() {
        let (execution, client, quota) = bank();
        let hold = client
            .try_reserve_external(JobCost {
                input_bytes: CATALOGUE_BYTES,
                result_bytes: CATALOGUE_BYTES,
            })
            .unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let native = Arc::clone(&entered);
        let result = discover_with(client, move |_| {
            native.store(true, Ordering::SeqCst);
            Ok(empty())
        })
        .await;
        assert!(result.is_err());
        assert!(!entered.load(Ordering::SeqCst));
        drop(hold);
        assert_eq!(quota.snapshot().jobs, 0);
        join(execution);
    }

    #[tokio::test]
    async fn cancelling_observation_keeps_running_native_work_admitted() {
        let (execution, client, quota) = bank();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(discover_with(client, move |context| {
            let _ = entered_tx.send(());
            release_rx.recv().unwrap();
            assert!(context.stop_requested());
            Ok(empty())
        }));
        entered_rx.await.unwrap();
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        let admitted_while_blocked = quota.snapshot().jobs;
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while quota.snapshot().jobs != 0 {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        join(execution);
        assert_eq!(admitted_while_blocked, 1);
    }

    #[test]
    fn byte_audit_includes_spare_device_name_storage() {
        let mut catalogue = empty();
        let baseline = catalogue.retained_bytes().unwrap();
        catalogue.inputs = Vec::with_capacity(4);
        catalogue.inputs.push(String::with_capacity(4096));
        assert!(catalogue.retained_bytes().unwrap() >= baseline + 4096 + 4 * size_of::<String>());
    }
}
