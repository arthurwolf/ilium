//! Best-effort startup display, serialized on the existing finite I/O bank.
use crate::{execution::ExecutionClient, state::ServerState};
use ilium_execution::{Job, JobContext, JobCost, Lane, Reservation, Retained};
use ilium_ipc::StartupProgress;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

const PATH_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 8 * 1024;

pub(crate) struct Publisher {
    client: ExecutionClient,
    native: Arc<Mutex<NativeOwner>>,
    generation: AtomicU64,
    sequence: AtomicU64,
}

struct NativeOwner {
    generation: u64,
    sequence: u64,
    finished: bool,
    // The original path allocation remains charged until its final callback.
    path: Retained<PathBuf>,
}

pub(crate) struct Phase {
    generation: u64,
    publisher: Arc<Publisher>,
}

enum Command {
    Publish(StartupProgress),
    Finish,
}

struct Write {
    native: Arc<Mutex<NativeOwner>>,
    generation: u64,
    sequence: u64,
    command: Command,
    #[cfg(test)]
    entered: Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>,
}

// Captured strings must drop before their admission if never submitted.
struct PreparedWrite {
    job: Write,
    reservation: Reservation,
}

impl Job for Write {
    type Output = ();
    type Error = io::Error;

    fn run(self, _context: JobContext) -> Result<(), io::Error> {
        #[cfg(test)]
        if let Some(entered) = self.entered {
            let _ = entered.send(std::thread::current().id());
        }
        // Only native callbacks acquire this mutex. Coordination never waits
        // for a filesystem operation while acquiring a shared registry lock.
        let mut native = self
            .native
            .lock()
            .map_err(|_| io::Error::other("startup publication owner poisoned"))?;
        if self.generation < native.generation {
            return Ok(());
        }
        // Finish closes its phase even when it waited for admission while a
        // younger update returned. Only publications coalesce by sequence.
        if self.generation == native.generation
            && (native.finished
                || (matches!(&self.command, Command::Publish(_))
                    && self.sequence <= native.sequence))
        {
            return Ok(());
        }
        native.generation = self.generation;
        native.sequence = self.sequence;
        match self.command {
            Command::Publish(progress) => {
                native.finished = false;
                ilium_ipc::publish_startup_progress(native.path.view(), &progress)
            }
            Command::Finish => {
                native.finished = true;
                ilium_ipc::clear_startup_progress(native.path.view());
                Ok(())
            }
        }
    }
}

// No wrapping identifiers, and no post-MSRV atomic APIs.
fn next(counter: &AtomicU64) -> Option<u64> {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        let value = current.checked_add(1)?;
        match counter.compare_exchange_weak(current, value, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(value),
            Err(actual) => current = actual,
        }
    }
}

impl Publisher {
    pub(crate) async fn bind(state: &ServerState, socket_path: &Path) -> Option<Phase> {
        if let Some(publisher) = state.startup_progress.get() {
            return publisher.begin();
        }
        let publisher = Self::new(state.execution.get()?.client.clone(), socket_path).await?;
        let phase = publisher.begin();
        // The composition root calls bind once before publishing State.
        state.startup_progress.set(publisher).ok()?;
        phase
    }

    async fn new(client: ExecutionClient, socket_path: &Path) -> Option<Arc<Self>> {
        if socket_path.as_os_str().len() > PATH_BYTES / 4 {
            return None;
        }
        let input_bytes = socket_path
            .as_os_str()
            .len()
            .checked_mul(4)?
            .checked_add(PATH_BYTES)?;
        let reservation = client
            .foundation
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes,
                    result_bytes: PATH_BYTES,
                },
            )
            .ok()?;
        let socket_path = socket_path.to_path_buf();
        let path = client
            .run_reserved(reservation, move |_context| {
                let path = ilium_ipc::startup_progress_path(&socket_path);
                ilium_ipc::clear_startup_progress(&path);
                Ok::<_, io::Error>(path)
            })
            .await
            .ok()?;
        let publisher = Arc::new(Self {
            client,
            native: Arc::new(Mutex::new(NativeOwner {
                generation: 0,
                sequence: 0,
                finished: false,
                path,
            })),
            generation: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
        });
        Some(publisher)
    }

    pub(crate) fn begin(self: &Arc<Self>) -> Option<Phase> {
        Some(Phase {
            generation: next(&self.generation)?,
            publisher: Arc::clone(self),
        })
    }
}

impl Phase {
    fn prepare(
        &self,
        category: &str,
        item: &str,
        completed: usize,
        total: usize,
    ) -> Option<PreparedWrite> {
        let text_bytes = category.len().checked_add(item.len())?;
        if text_bytes > MAX_TEXT_BYTES {
            return None; // Courtesy display refusal preserves the actual pane.
        }
        let input_bytes = text_bytes.checked_mul(8)?.checked_add(PATH_BYTES)?;
        let reservation = self
            .publisher
            .client
            .foundation
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes,
                    result_bytes: 4096,
                },
            )
            .ok()?;
        let job = Write {
            native: Arc::clone(&self.publisher.native),
            generation: self.generation,
            sequence: next(&self.publisher.sequence)?,
            #[cfg(test)]
            entered: None,
            command: Command::Publish(StartupProgress {
                category: category.to_owned(),
                item: item.to_owned(),
                completed: completed as u64,
                total: total as u64,
            }),
        };
        Some(PreparedWrite { job, reservation })
    }

    async fn submit(&self, prepared: PreparedWrite) {
        let PreparedWrite { job, reservation } = prepared;
        if let Err(error) = self.publisher.client.run_reserved(reservation, job).await {
            tracing::debug!(%error, "startup progress was not published");
        }
    }

    pub(crate) async fn publish(&self, category: &str, item: &str, completed: usize, total: usize) {
        if let Some(prepared) = self.prepare(category, item, completed, total) {
            self.submit(prepared).await;
        }
    }

    pub(crate) async fn publish_pane(
        &self,
        state: &ServerState,
        node_id: ilium_core::NodeId,
        completed: usize,
        total: usize,
    ) {
        let prepared = {
            let tree = state.tree.read().await;
            let name = tree.get(node_id).map_or("pane", |node| node.name.as_str());
            self.prepare("Restoring panes", name, completed, total)
        };
        if let Some(prepared) = prepared {
            self.submit(prepared).await;
        }
    }

    pub(crate) async fn finish(&self) {
        let Some(sequence) = next(&self.publisher.sequence) else {
            return;
        };
        let Ok(reservation) = self
            .publisher
            .client
            .reserve(
                Lane::Io,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
            )
            .await
        else {
            return;
        };
        self.submit(PreparedWrite {
            job: Write {
                native: Arc::clone(&self.publisher.native),
                generation: self.generation,
                sequence,
                command: Command::Finish,
                #[cfg(test)]
                entered: None,
            },
            reservation,
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::ServerExecution;

    #[tokio::test]
    async fn native_publication_runs_off_caller_and_preserves_record_readback() {
        let bank = ServerExecution::start().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("session.sock");
        let path = ilium_ipc::startup_progress_path(&socket);
        let publisher = Publisher::new(bank.client.clone(), &socket).await.unwrap();
        let phase = publisher.begin().unwrap();
        let mut prepared = phase
            .prepare("Restoring panes", "Exact pane", 2, 7)
            .unwrap();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        prepared.job.entered = Some(sender);
        phase.submit(prepared).await;
        assert_ne!(receiver.await.unwrap(), std::thread::current().id());
        assert_eq!(
            ilium_ipc::read_startup_progress(&path),
            Some(StartupProgress {
                category: "Restoring panes".into(),
                item: "Exact pane".into(),
                completed: 2,
                total: 7,
            })
        );
        phase.finish().await;
        assert!(!path.exists());
        bank.request_shutdown();
    }

    #[tokio::test]
    async fn finish_fences_a_late_write_and_cannot_clear_a_newer_phase() {
        let bank = ServerExecution::start().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("session.sock");
        let path = ilium_ipc::startup_progress_path(&socket);
        let publisher = Publisher::new(bank.client.clone(), &socket).await.unwrap();
        let old = publisher.begin().unwrap();
        let late = old.prepare("Old", "Delayed", 0, 1).unwrap();
        old.finish().await;
        old.submit(late).await;
        assert!(
            !path.exists(),
            "a retired phase cannot resurrect the marker"
        );
        let current = publisher.begin().unwrap();
        current.publish("Current", "Retained", 1, 1).await;
        old.finish().await;
        assert_eq!(
            ilium_ipc::read_startup_progress(&path).unwrap().item,
            "Retained"
        );
        current.finish().await;
        assert!(!path.exists());
        bank.request_shutdown();
    }

    #[tokio::test]
    async fn finish_closes_its_phase_even_if_a_younger_update_returns_first() {
        let bank = ServerExecution::start().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("session.sock");
        let path = ilium_ipc::startup_progress_path(&socket);
        let publisher = Publisher::new(bank.client.clone(), &socket).await.unwrap();
        let phase = publisher.begin().unwrap();
        // Capture the finish ordinal before its asynchronous admission wait.
        let sequence = next(&publisher.sequence).unwrap();
        phase.publish("Restoring", "Younger update", 1, 1).await;
        assert!(path.exists());
        let reservation = bank
            .client
            .foundation
            .try_reserve(
                Lane::Io,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
            )
            .unwrap();
        phase
            .submit(PreparedWrite {
                job: Write {
                    native: Arc::clone(&publisher.native),
                    generation: phase.generation,
                    sequence,
                    command: Command::Finish,
                    entered: None,
                },
                reservation,
            })
            .await;
        assert!(
            !path.exists(),
            "finish must not be superseded by its own phase's update"
        );
        bank.request_shutdown();
    }

    #[tokio::test]
    async fn cancellation_keeps_a_blocked_callback_admitted_until_native_return() {
        let bank = ServerExecution::start().unwrap();
        let quota = bank.quota_group();
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("session.sock");
        let path = ilium_ipc::startup_progress_path(&socket);
        let publisher = Publisher::new(bank.client.clone(), &socket).await.unwrap();
        let phase = publisher.begin().unwrap();
        let baseline = quota.snapshot().jobs;
        let native = Arc::clone(&publisher.native);
        let (locked_sender, locked_receiver) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let parking_client = bank.client.clone();
        let parked = tokio::spawn(async move {
            parking_client
                .run(
                    Lane::Io,
                    JobCost {
                        input_bytes: 4096,
                        result_bytes: 4096,
                    },
                    move |_| {
                        // A second admitted bank job owns the test-only blocker;
                        // the test's coordination future holds no native mutex.
                        let blocker = native.lock().unwrap();
                        locked_sender.send(()).unwrap();
                        released.recv().unwrap();
                        drop(blocker);
                        Ok::<_, io::Error>(())
                    },
                )
                .await
        });
        locked_receiver.await.unwrap();
        let mut prepared = phase.prepare("Blocked", "Original", 0, 1).unwrap();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        prepared.job.entered = Some(sender);
        let task = tokio::spawn(async move { phase.submit(prepared).await });
        receiver.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(quota.snapshot().jobs, baseline + 2);
        release.send(()).unwrap();
        drop(parked.await.unwrap().unwrap());
        publisher.begin().unwrap().finish().await;
        assert!(!path.exists());
        let completion = bank.client.completion_notification();
        loop {
            let notified = completion.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if quota.snapshot().jobs == baseline {
                break;
            }
            notified.await;
        }
        bank.request_shutdown();
    }
}
