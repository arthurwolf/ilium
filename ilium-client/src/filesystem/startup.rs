//! Replaceable startup observations on the existing finite filesystem bank.
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, Retained,
};
use ilium_ipc::StartupProgress;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

const READ_INTERVAL: Duration = Duration::from_millis(100);
const MAX_PATH_BYTES: usize = 64 * 1024;
const COST: JobCost = JobCost {
    input_bytes: 256 * 1024,
    result_bytes: 256 * 1024,
};

struct ReadStartup(PathBuf);
struct Observation {
    path: PathBuf,
    progress: Option<StartupProgress>,
    #[cfg(test)]
    worker_thread: std::thread::ThreadId,
}
impl Job for ReadStartup {
    type Output = Observation;
    type Error = ();
    fn run(self, context: JobContext) -> Result<Observation, ()> {
        if context.stop_requested() {
            return Err(());
        }
        let progress = ilium_ipc::read_startup_progress(&self.0);
        if context.stop_requested() {
            return Err(());
        }
        Ok(Observation {
            path: self.0,
            progress,
            #[cfg(test)]
            worker_thread: std::thread::current().id(),
        })
    }
}

/// One active read and one immutable charged observation. Refusal keeps the
/// previous observation; it never falls back to a read on the UI thread.
#[derive(Default)]
pub(crate) struct StartupReader {
    client: Option<Client>,
    active: Option<Receipt<ReadStartup>>,
    observation: Option<Retained<Observation>>,
    next_read_at: Option<Instant>,
    closed: bool,
}
impl StartupReader {
    pub(crate) fn configure(&mut self, client: Client, wake: Arc<tokio::sync::Notify>) {
        self.client = Some(client.with_completion_wake(move || wake.notify_one()));
    }
    pub(crate) fn progress(&self) -> Option<&StartupProgress> {
        self.observation.as_ref()?.view().progress.as_ref()
    }
    pub(crate) fn refresh(&mut self, path: &Path, now: Instant) {
        if self.closed {
            return;
        }
        if let Some(receipt) = self.active.as_mut() {
            match receipt.try_take() {
                JobPoll::Pending => return,
                JobPoll::Ready(result) => {
                    self.active = None;
                    let observation = result.map(|outcome| match outcome {
                        JobOutcome::Finished(Ok(value)) => Some(value),
                        _ => None,
                    });
                    let (value, retention) = observation.into_parts();
                    if let Some(value) = value.filter(|value| value.path == path) {
                        self.observation = Some(retention.retain(value));
                    }
                }
                JobPoll::Lost | JobPoll::Taken => {
                    self.active = None;
                }
            }
        }
        if self.next_read_at.is_some_and(|deadline| now < deadline) {
            return;
        }
        self.next_read_at = Some(now + READ_INTERVAL);
        if path.as_os_str().len() > MAX_PATH_BYTES {
            return;
        }
        let Some(client) = self.client.as_ref() else {
            return;
        };
        let Ok(reservation) = client.try_reserve(Lane::Io, COST) else {
            return;
        };
        // Admission precedes the only path copy. No thread or queue is added.
        if let Ok(receipt) = reservation.submit(ReadStartup(path.to_path_buf())) {
            self.active = Some(receipt);
        }
    }
    pub(crate) fn close(&mut self) {
        self.closed = true;
        if let Some(receipt) = self.active.take() {
            receipt.cancel();
        }
        self.observation = None;
    }
}
impl Drop for StartupReader {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_observation_runs_on_bank_and_close_releases_display() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.startup");
        let expected = StartupProgress {
            category: "Restoring panes".into(),
            item: "editor".into(),
            completed: 2,
            total: 5,
        };
        ilium_ipc::publish_startup_progress(&path, &expected).unwrap();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let client = crate::execution::test_client().with_completion_wake(move || {
            let _ = send.try_send(());
        });
        let mut reader = StartupReader::default();
        reader.client = Some(client);
        let now = Instant::now();
        reader.refresh(&path, now);
        assert!(reader.active.is_some());
        assert!(reader.progress().is_none());
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        reader.refresh(&path, now);
        assert_eq!(reader.progress(), Some(&expected));
        assert_ne!(
            reader.observation.as_ref().unwrap().view().worker_thread,
            std::thread::current().id()
        );
        assert!(
            reader.active.is_none(),
            "cadence must not submit another read immediately"
        );
        reader.close();
        reader.refresh(&path, now + READ_INTERVAL);
        assert!(reader.progress().is_none());
        assert!(reader.active.is_none());
    }
    #[test]
    fn oversized_file_is_refused_without_retaining_a_partial_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.startup");
        std::fs::write(&path, vec![b'x'; 64 * 1024 + 1]).unwrap();
        assert!(ilium_ipc::read_startup_progress(&path).is_none());
    }
}
